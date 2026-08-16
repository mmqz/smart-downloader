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

#include <cstring>
#include <string>

struct lt_session {
    lt::session ses;
    std::string save_path;
    explicit lt_session(const char* path)
        : ses(lt::session_params())
        , save_path(path ? path : "")
    {
        // M0 用默认设置（DHT/UPnP/uTP 默认开）；resume 持久化由 Rust 承载（D16），
        // 会话不写任何 *.resume（M1 再加 lt_request_save_resume 显式流程）。
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

} // extern "C"
