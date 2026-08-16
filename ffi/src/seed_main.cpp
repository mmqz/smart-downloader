// ffi/src/seed_main.cpp — 本地测试 seeder（M0 E2E 用，免外部工具/tracker）
// 行为：生成 2MB 确定性文件 → 建种子（v1）→ 监听指定端口做种 → 输出一行
//       "SEED <magnet>" 后常驻；客户端用 lt_add_peer(127.0.0.1:<port>) 直连注入。
// 用法：seed_main <port> <save_dir>

#include <libtorrent/session.hpp>
#include <libtorrent/session_params.hpp>
#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/torrent_info.hpp>
#include <libtorrent/create_torrent.hpp>
#include <libtorrent/hex.hpp>

#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <string>
#include <thread>
#include <chrono>
#include <fstream>
#include <memory>

namespace {
constexpr const char* kFileName = "m0_test.bin";
constexpr std::int64_t kFileSize = 2 * 1024 * 1024; // 2MB

void make_file(const std::string& dir) {
    std::ofstream f(dir + "/" + kFileName, std::ios::binary | std::ios::trunc);
    // 确定性伪随机（固定种子），保证跨运行可复现
    std::uint64_t x = 0x123456789abcdef0ULL;
    std::uint64_t remain = kFileSize;
    while (remain > 0) {
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        const std::uint64_t chunk = remain > 8 ? 8 : remain;
        f.write(reinterpret_cast<const char*>(&x), static_cast<std::streamsize>(chunk));
        remain -= chunk;
    }
}
} // namespace

int main(int argc, char** argv) {
    const int port = argc > 1 ? std::atoi(argv[1]) : 16889;
    const std::string dir = argc > 2 ? argv[2] : ".";

    try {
        make_file(dir);

        lt::file_storage fs;
        fs.add_file(kFileName, kFileSize);
        lt::create_torrent t(fs);
        // 不填 tracker：客户端靠 lt_add_peer 直连注入（本地确定性测试）
        lt::torrent_info ti = t.generate();
        const lt::sha1_hash v1 = ti.info_hashes().v1;

        lt::settings_pack sp;
        sp.set_int(lt::settings_pack::listen_port, port);
        sp.set_bool(lt::settings_pack::enable_upnp, false);
        sp.set_bool(lt::settings_pack::enable_natpmp, false);
        lt::session ses(sp);

        lt::add_torrent_params p;
        p.ti = std::make_shared<lt::torrent_info>(ti);
        p.save_path = dir;
        ses.add_torrent(p); // 文件已存在且完整 → 直接做种

        char ih[41];
        lt::aux::to_hex({v1.data(), v1.size()}, ih);
        std::printf("SEED magnet:?xt=urn:btih:%s PORT %d\n", ih, port);
        std::fflush(stdout);

        for (;;) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }
    } catch (const std::exception& e) {
        std::fprintf(stderr, "seed_main error: %s\n", e.what());
        return 1;
    }
}
