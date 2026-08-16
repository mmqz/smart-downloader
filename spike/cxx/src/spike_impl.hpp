// spike/cxx/src/spike_impl.hpp — approach B C++ 侧（cxx 用）
#pragma once

#include "rust/cxx.h"
#include <cstdint>
#include <memory>
#include <string>

// bridge 生成的 struct（lib.rs.h 声明，本头文件声明同名为两方共享）
struct Alert {
    std::int32_t kind;
    rust::String ih;
    rust::String msg;
    std::int64_t at;
};

class Session {
public:
    explicit Session(std::string save_path);
    ~Session();

    // 不可拷贝
    Session(const Session&) = delete;
    Session& operator=(const Session&) = delete;

    rust::String add_magnet(const char* magnet);
    std::pair<float, std::int32_t> status(rust::Str ih);
    rust::Vec<Alert> pop_alerts();
    void set_session_mask(std::uint32_t mask);

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

std::unique_ptr<Session> new_session(const char* save_path, const char* session_id);