// spike/cxx/src/spike_impl.hpp — approach B C++ 侧（cxx 用）
#pragma once

#include "rust/cxx.h"
#include <cstdint>
#include <memory>
#include <string>
#include <type_traits>

// Alert / Status 是 bridge 中声明的共享 struct。cxx 的 C++-side 共享 struct 模式：
// 由本头文件定义,并声明与 cxxbridge 生成代码相同的 CXXBRIDGE1_STRUCT_* 守卫宏,
// 这样 lib.rs.h 中的生成定义会被跳过,避免重复定义 (C2011)。
#ifndef CXXBRIDGE1_STRUCT_Status
#define CXXBRIDGE1_STRUCT_Status
struct Status final {
    float progress;
    std::int32_t state;
    using IsRelocatable = ::std::true_type;
};
#endif

#ifndef CXXBRIDGE1_STRUCT_Alert
#define CXXBRIDGE1_STRUCT_Alert
struct Alert final {
    std::int32_t kind;
    rust::String ih;
    rust::String msg;
    std::int64_t at;
    using IsRelocatable = ::std::true_type;
};
#endif

class Session {
public:
    explicit Session(std::string save_path);
    ~Session();

    // 不可拷贝
    Session(const Session&) = delete;
    Session& operator=(const Session&) = delete;

    // cxx 中 &self 接收者对应 C++ const 方法
    rust::String add_magnet(rust::Str magnet) const;
    Status status(rust::Str ih) const;
    rust::Vec<Alert> pop_alerts() const;
    void set_session_mask(std::uint32_t mask) const;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

std::unique_ptr<Session> new_session(rust::Str save_path, rust::Str session_id);