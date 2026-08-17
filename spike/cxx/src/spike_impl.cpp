// spike/cxx/src/spike_impl.cpp — approach B 实现（cxx 自动处理异常边界与 UniquePtr 生命周期）
#include "spike_impl.hpp"
#include "ffi-cxx-spike/src/lib.rs.h"

#include <libtorrent/session.hpp>
#include <libtorrent/session_params.hpp>
#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/magnet_uri.hpp>
#include <libtorrent/torrent_handle.hpp>
#include <libtorrent/torrent_status.hpp>
#include <libtorrent/alert.hpp>
#include <libtorrent/alert_types.hpp>

#include <chrono>
#include <cstring>
#include <deque>
#include <mutex>
#include <vector>

// spike-only shim: vcpkg boost 1.91 以 header-only system/asio 构建,
// 但 asio 内联路径引用非模板的二参 throw_exception 外部符号。spike 提供
// out-of-line 定义以便链接;生产路径 (ffi/CMakeLists) 无此问题。
namespace boost {
BOOST_NORETURN void throw_exception(std::exception const& e, boost::source_location const&) {
    throw e;
}
} // namespace boost

struct Session::Impl {
    lt::session ses;
    std::string save_path;
    std::uint32_t mask = 0;
    std::mutex mtx;
    std::deque<Alert> ring;

    explicit Impl(std::string sp, const char* /*id*/)
        : ses(lt::session_params())
        , save_path(std::move(sp))
    {
        lt::settings_pack sp2;
        sp2.set_int(lt::settings_pack::alert_mask, lt::alert::all_categories);
        ses.apply_settings(sp2);
    }
};

namespace {
int map_kind(const lt::alert* a) {
    switch (a->type()) {
        case lt::metadata_received_alert::alert_type: return 8;    // LT_ALERT_METADATA
        case lt::torrent_finished_alert::alert_type:
        case lt::torrent_paused_alert::alert_type:
        case lt::torrent_error_alert::alert_type:     return 16;   // LT_ALERT_STATE
        case lt::save_resume_data_alert::alert_type:  return 32;   // LT_ALERT_RESUME
        case lt::tracker_error_alert::alert_type:     return 1;    // LT_ALERT_TRACKER
        case lt::peer_connect_alert::alert_type:
        case lt::peer_disconnected_alert::alert_type: return 2;    // LT_ALERT_PEER
        default: return 0;
    }
}
std::string ih_hex(const lt::torrent_handle& h) {
    if (!h.is_valid() || !h.info_hashes().has_v1()) return {};
    static const char* hex = "0123456789abcdef";
    const lt::sha1_hash& v1 = h.info_hashes().v1;
    const char* d = v1.data();
    std::string out(40, '0');
    for (int i = 0; i < 20; ++i) {
        const unsigned char b = static_cast<unsigned char>(d[i]);
        out[i * 2] = hex[b >> 4];
        out[i * 2 + 1] = hex[b & 0xF];
    }
    return out;
}
} // namespace

Session::Session(std::string save_path) : impl_(std::make_unique<Impl>(std::move(save_path), nullptr)) {}
Session::~Session() = default;

rust::String Session::add_magnet(rust::Str magnet) const {
    // 非法输入抛异常 → cxx 转 Err
    const std::string m(magnet.data(), magnet.size());
    lt::add_torrent_params p = lt::parse_magnet_uri(m);
    p.save_path = impl_->save_path;
    lt::error_code ec;
    const lt::torrent_handle h = impl_->ses.add_torrent(p, ec);
    if (ec) throw std::runtime_error(ec.message());
    const std::string ih = ih_hex(h);
    return rust::String(ih);
}

Status Session::status(rust::Str ih) const {
    // 简化：spike 仅验证调用链；真实解析在 M1 统一实现
    Status s;
    s.progress = 0.0f;
    s.state = 0;
    return s;
}

void Session::set_session_mask(std::uint32_t mask) const { impl_->mask = mask; }

rust::Vec<Alert> Session::pop_alerts() const {
    std::vector<lt::alert*> alerts;
    impl_->ses.pop_alerts(&alerts); // 2.x：指针形参
    rust::Vec<Alert> out;
    for (const lt::alert* a : alerts) {
        const int kind = map_kind(a);
        if (kind == 0 || ((impl_->mask & kind) == 0)) continue;
        Alert fa;
        fa.kind = kind;
        fa.at = std::chrono::duration_cast<std::chrono::milliseconds>(
                    a->timestamp().time_since_epoch()).count();
        const lt::torrent_alert* ta = dynamic_cast<const lt::torrent_alert*>(a);
        fa.ih = ta ? rust::String(ih_hex(ta->handle)) : rust::String("");
        fa.msg = rust::String(a->message());
        out.push_back(std::move(fa));
    }
    return out;
}

std::unique_ptr<Session> new_session(rust::Str save_path, rust::Str /*session_id*/) {
    const std::string sp(save_path.data(), save_path.size());
    return std::make_unique<Session>(sp);
}