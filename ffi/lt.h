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

/* M0 补充：本地测试用 peer 注入（本地 seeder 直连，无需 tracker） */
lt_err lt_add_peer(lt_session* s, const char* ih, const char* ip, uint16_t port);

/* —— M0/spike：alert 队列（D31 预算 ≤12 种；扁平化值拷贝，所有权归 wrapper；溢出计数）—— */
typedef enum {
    LT_ALERT_TRACKER  = 1,
    LT_ALERT_PEER     = 2,
    LT_ALERT_ERROR    = 4,
    LT_ALERT_METADATA = 8,
    LT_ALERT_STATE    = 16,
    LT_ALERT_RESUME   = 32,
    LT_ALERT_PIECE    = 64
} lt_alert_mask;

typedef struct {
    int   kind;            /* 对应 mask 位 */
    char  ih[41];          /* 相关 torrent infohash（非 torrent 类为空串） */
    char  msg[512];        /* 人类可读（tracker 错误/peer 断开原因/resume 失败原因…） */
    int64_t at;            /* 毫秒时间戳 */
    int   resume_ready;    /* RESUME 时：1=可调 lt_take_resume_data */
} lt_alert;

lt_err lt_set_alert_mask(lt_session* s, const char* ih, uint32_t mask);
lt_err lt_pop_alerts(lt_session* s, lt_alert* buf, size_t cap, size_t* out_count);
lt_err lt_alerts_dropped(lt_session* s, uint32_t* out);

#ifdef __cplusplus
}
#endif

#endif /* SMART_DL_LT_H */
