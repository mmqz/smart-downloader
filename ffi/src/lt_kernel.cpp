// ffi/src/lt_kernel.cpp — 手写 C ABI 方案 A 实现（M0 子集，5 函数）
// 契约：ffi/lt.h（M0 子集）。完整契约见设计文档 v0.6 §8.3。
// 内存规则（D13）：输出缓冲 Rust 预分配 + capacity；无 new[]/静态缓冲/所有权转移。
// 注：本文件在 libtorrent 2.x API 上编写；编译期以真实头文件为准迭代修正。

#include "lt.h"

#include <libtorrent/session.hpp>
#include <libtorrent/session_params.hpp>
#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/magnet_uri.hpp>
#include <libtorrent/torrent_handle.hpp>
#include <libtorrent/torrent_status.hpp>
#include <libtorrent/alert.hpp>
#include <libtorrent/alert_types.hpp>

#include <chrono>
#include <cstdint>
#include <cstring>
#include <deque>
#include <mutex>
#include <string>
#include <vector>

namespace {
constexpr size_t kAlertRingCap = 1024;
}

struct lt_session {
    lt::session ses;
    std::string save_path;
    std::mutex mtx;                 // 保护 ring/dropped
    std::deque<lt_alert> ring;      // 扁平化值拷贝（生命周期归 wrapper，Rust 立即再拷贝）
    uint32_t dropped = 0;           // 溢出 + 非预算类型 计数
    uint32_t mask = 0;              // 当前过滤 mask

    explicit lt_session(const char* path)
        : ses(lt::session_params())
        , save_path(path ? path : "")
    {
        // M0：开全量 alert 类别，由 wrapper 按 mask 过滤（M1 再做精细配置）
        lt::settings_pack sp;
        sp.set_int(lt::settings_pack::alert_mask, lt::alert::all_categories);
        ses.apply_settings(sp);
    }
};

namespace {

// ih(40 hex) -> torrent_handle；非法输入/未找到 -> 无效 handle
lt::torrent_handle find_handle(lt_session* s, const char* ih) {
    if (!s || !ih || std::strlen(ih) != 40) return {};
    lt::sha1_hash h;
    auto nibble = [](char c) -> int {
        if (c >= '0' && c <= '9') return c - '0';
        if (c >= 'a' && c <= 'f') return c - 'a' + 10;
        if (c >= 'A' && c <= 'F') return c - 'A' + 10;
        return -1;
    };
    for (int i = 0; i < 20; ++i) {
        const int hi = nibble(ih[i * 2]);
        const int lo = nibble(ih[i * 2 + 1]);
        if (hi < 0 || lo < 0) return {};
        h[i] = static_cast<unsigned char>((hi << 4) | lo);
    }
    return s->ses.find_torrent(lt::info_hash_t(h));
}

void hex_encode_v1(const lt::sha1_hash& v1, char* out /*41*/) {
    static const char* hex = "0123456789abcdef";
    for (int i = 0; i < 20; ++i) {
        const unsigned char b = static_cast<unsigned char>(v1[i]);
        out[i * 2] = hex[b >> 4];
        out[i * 2 + 1] = hex[b & 0xF];
    }
    out[40] = '\0';
}

// —— alert 扁平化（D31 预算子集；其余类型丢弃并计数）——
int map_alert_kind(const lt::alert* a) {
    switch (a->type()) {
        case lt::metadata_received_alert::alert_type:     return LT_ALERT_METADATA;
        case lt::torrent_finished_alert::alert_type:
        case lt::torrent_paused_alert::alert_type:
        case lt::torrent_error_alert::alert_type:         return LT_ALERT_STATE;
        case lt::save_resume_data_alert::alert_type:
        case lt::save_resume_data_failed_alert::alert_type: return LT_ALERT_RESUME;
        case lt::tracker_error_alert::alert_type:         return LT_ALERT_TRACKER;
        case lt::peer_connected_alert::alert_type:
        case lt::peer_disconnected_alert::alert_type:     return LT_ALERT_PEER;
        case lt::piece_finished_alert::alert_type:        return LT_ALERT_PIECE;
        default:                                          return 0;
    }
}

void fill_ih_from_torrent_alert(const lt::alert* a, char out[41]) {
    out[0] = '\0';
    const auto* ta = dynamic_cast<const lt::torrent_alert*>(a);
    if (ta && ta->handle.is_valid() && ta->handle.info_hash().has_v1()) {
        hex_encode_v1(ta->handle.info_hash().v1, out);
    }
}

void drain_session(lt_session* s) {
    std::vector<lt::alert*> alerts;
    s->ses.pop_alerts(alerts);
    if (alerts.empty()) return;
    std::lock_guard<std::mutex> lk(s->mtx);
    for (const lt::alert* a : alerts) {
        const int kind = map_alert_kind(a);
        if (kind == 0 || ((s->mask & kind) == 0)) { ++s->dropped; continue; }
        lt_alert fa{};
        fa.kind = kind;
        fa.at = std::chrono::duration_cast<std::chrono::milliseconds>(
                    a->timestamp().time_since_epoch()).count();
        fill_ih_from_torrent_alert(a, fa.ih);
        const std::string m = a->message();
        std::strncpy(fa.msg, m.c_str(), sizeof(fa.msg) - 1);
        fa.msg[sizeof(fa.msg) - 1] = '\0';
        if (const auto* r = dynamic_cast<const lt::save_resume_data_alert*>(a)) {
            fa.resume_ready = r->resume_data ? 1 : 0;
        }
        if (s->ring.size() >= kAlertRingCap) { s->ring.pop_front(); ++s->dropped; }
        s->ring.push_back(fa);
    }
}

} // namespace

extern "C" {

lt_err lt_session_new(const char* save_path, const char* /*session_id*/, lt_session** out) {
    if (!out) return LT_ERR_ARG;
    try {
        *out = new lt_session(save_path);
        return LT_OK;
    } catch (...) {
        return LT_ERR_ENGINE;
    }
}

void lt_session_free(lt_session* s) { delete s; }

lt_err lt_add_magnet(lt_session* s, const char* magnet, const char** web_seeds, char* ih_out /*41*/) {
    if (!s || !magnet || !ih_out) return LT_ERR_ARG;
    try {
        lt::add_torrent_params p = lt::parse_magnet_uri(magnet);
        p.save_path = s->save_path;
        if (web_seeds) {
            for (const char** ws = web_seeds; *ws != nullptr; ++ws) {
                p.url_seeds.emplace_back(*ws);
            }
        }
        const lt::torrent_handle h = s->ses.add_torrent(p);
        const lt::info_hash_t ih = h.info_hash();
        if (!ih.has_v1()) {
            // v2-only magnet：M0 不支持（设计文档注记），报 ENGINE 错误
            return LT_ERR_ENGINE;
        }
        hex_encode_v1(ih.v1, ih_out);
        return LT_OK;
    } catch (...) {
        return LT_ERR_ENGINE;
    }
}

lt_err lt_status(lt_session* s, const char* ih, lt_torrent_status* out) {
    if (!s || !ih || !out) return LT_ERR_ARG;
    try {
        const lt::torrent_handle h = find_handle(s, ih);
        if (!h.is_valid()) return LT_ERR_NOT_FOUND;
        const lt::torrent_status st = h.status();
        out->metadata_received = st.has_metadata ? 1 : 0;
        switch (st.state) {
            case lt::torrent_status::downloading_metadata:
                out->state = 4; // 元数据获取中（F2 三阶段）
                break;
            case lt::torrent_status::downloading:
            case lt::torrent_status::checking_files:
            case lt::torrent_status::checking_resume_data:
            case lt::torrent_status::allocating:
                out->state = 0; // 下载中
                break;
            case lt::torrent_status::finished:
            case lt::torrent_status::seeding:
                out->state = 1; // 完成
                break;
            case lt::torrent_status::paused:
                out->state = 2; // 暂停
                break;
            default:
                out->state = 3; // 错误/其他
                break;
        }
        out->progress = st.progress;
        out->downloaded = st.total_wanted_done;
        out->total = st.total_wanted;
        out->down_rate = st.download_rate;
        out->up_rate = st.upload_rate;
        out->num_peers = st.num_peers;
        out->num_seeds = st.num_seeds;
        return LT_OK;
    } catch (...) {
        return LT_ERR_ENGINE;
    }
}

lt_err lt_add_peer(lt_session* s, const char* ih, const char* ip, uint16_t port) {
    if (!s || !ih || !ip) return LT_ERR_ARG;
    try {
        const lt::torrent_handle h = find_handle(s, ih);
        if (!h.is_valid()) return LT_ERR_NOT_FOUND;
        lt::error_code ec;
        lt::tcp::endpoint ep(lt::make_address(ip, ec), port);
        if (ec) return LT_ERR_ARG;
        h.add_peer(ep);
        return LT_OK;
    } catch (...) {
        return LT_ERR_ENGINE;
    }
}

lt_err lt_set_alert_mask(lt_session* s, const char* /*ih*/, uint32_t mask) {
    if (!s) return LT_ERR_ARG;
    std::lock_guard<std::mutex> lk(s->mtx);
    s->mask = mask;
    return LT_OK;
}

lt_err lt_pop_alerts(lt_session* s, lt_alert* buf, size_t cap, size_t* out_count) {
    if (!s || !buf || !out_count) return LT_ERR_ARG;
    try {
        drain_session(s);
        std::lock_guard<std::mutex> lk(s->mtx);
        const size_t n = s->ring.size() < cap ? s->ring.size() : cap;
        for (size_t i = 0; i < n; ++i) buf[i] = s->ring[i];
        s->ring.erase(s->ring.begin(), s->ring.begin() + static_cast<std::ptrdiff_t>(n));
        *out_count = n;
        return LT_OK;
    } catch (...) {
        return LT_ERR_ENGINE;
    }
}

lt_err lt_alerts_dropped(lt_session* s, uint32_t* out) {
    if (!s || !out) return LT_ERR_ARG;
    std::lock_guard<std::mutex> lk(s->mtx);
    *out = s->dropped;
    return LT_OK;
}

} // extern "C"
