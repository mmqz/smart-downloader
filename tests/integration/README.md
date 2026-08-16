# 集成测试基建（M0 起）

- `seed/`：本地真实 BT seeder（rqbit 或 libtorrent 自 seed，2MB 测试文件）→ M0 E2E / M5 兜底测试用
- `http_server.rs`：axum 可配置 test server（206/200/416/429/中途 404/ETag 变化）→ M4 用
- `ftp_server.rs`：最小 FTP server（PASV + REST）→ M4c 用
