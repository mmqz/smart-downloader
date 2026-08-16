/* ffi/lt.h — M0 子集（契约 v0.1，M0 spike 期间冻结；全量见设计文档 v0.6 §8.3）
 * 手写 C ABI 公共契约：C++ 侧 lt_kernel.cpp 实现；Rust 侧 bindgen 生成声明。
 * 内存规则（D13）：输出缓冲 Rust 预分配 + capacity；无 new[]/静态缓冲/所有权转移。
 */
#ifndef SMART_DL_LT_H
#define SMART_DL_LT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct lt_session lt_session;

typedef enum {
    LT_OK = 0,
    LT_ERR_ARG,
    LT_ERR_ENGINE,
    LT_ERR_IO,
    LT_ERR_NOT_FOUND,
    LT_ERR_BUFFER_TOO_SMALL
} lt_err;

/* —— M0 最小内核（5 函数）—— */
lt_err lt_session_new(const char* save_path, const char* session_id, lt_session** out);
void   lt_session_free(lt_session* s);

typedef struct {
    int     state;             /* 0 下载 1 完成 2 暂停 3 错误 4 元数据获取中 */
    float   progress;          /* 0..1（已下载/总字节，metadata 前恒 0） */
    int64_t downloaded, total, down_rate, up_rate;
    int     num_peers, num_seeds;   /* 已连接数（F2） */
    int     metadata_received;      /* F2 三阶段评估前提 */
} lt_torrent_status;

lt_err lt_add_magnet(lt_session* s, const char* magnet, const char** web_seeds, char* ih_out /*41 字节*/);
lt_err lt_status(lt_session* s, const char* ih, lt_torrent_status* out);

#ifdef __cplusplus
}
#endif

#endif /* SMART_DL_LT_H */
