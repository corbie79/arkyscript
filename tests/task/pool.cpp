#include "runtime/task/task_pool.h"
#include <atomic>
#include <chrono>
#include <future>
#include <iostream>
#include <set>

using namespace std::chrono_literals;
namespace {
void check(bool condition, const char* message) { if (!condition) throw std::runtime_error(message); }
template<class F> void fails(F&& call, const char* message) {
    try { call(); } catch (const std::exception& error) {
        check(std::string(error.what()).find(message) != std::string::npos, error.what());
        return;
    }
    throw std::runtime_error("expected failure");
}
void parallelism() {
    tynex::task_pool pool(2, 256);
    std::mutex mutex;
    std::condition_variable together;
    std::set<std::thread::id> ids;
    auto meet = [&](std::stop_token) {
        std::unique_lock lock(mutex);
        ids.insert(std::this_thread::get_id());
        together.notify_all();
        check(together.wait_for(lock, 5s, [&] { return ids.size() == 2; }), "two OS workers must overlap");
        return 21;
    };
    auto first = pool.submit(meet), second = pool.submit(meet);
    check(first.get() + second.get() == 42, "parallel results");
    check(first.get() == 21 && first.ready(), "repeatable wait");
    check(!ids.contains(std::this_thread::get_id()), "work left the caller thread");
    std::vector<tynex::task<void>> many;
    for (int i = 0; i < 128; ++i) many.push_back(pool.submit([&](std::stop_token) {
        std::lock_guard lock(mutex);
        ids.insert(std::this_thread::get_id());
    }));
    for (const auto& task : many) task.get();
    check(ids.size() == 2, "tasks reuse exactly two OS workers");
}
void cancellation_and_capacity() {
    tynex::task_pool pool(1, 1);
    std::promise<void> started;
    auto running = pool.submit([&](std::stop_token stop) {
        started.set_value();
        while (!stop.stop_requested()) std::this_thread::yield();
        return 99;
    });
    check(started.get_future().wait_for(5s) == std::future_status::ready, "worker starts");
    std::atomic<bool> executed = false;
    auto queued = pool.submit([&](std::stop_token) { executed = true; });
    check(queued.status() == tynex::task_status::queued, "queued status");
    fails([&] { pool.submit([](std::stop_token) {}); }, "queue is full");
    check(queued.cancel() && queued.ready(), "queued cancellation is immediate");
    fails([&] { queued.get(); }, "cancelled");
    auto replacement = pool.submit([](std::stop_token) { return 42; });
    check(running.cancel(), "running cancellation accepted");
    fails([&] { running.get(); }, "cancelled");
    check(!running.cancel(), "terminal cancellation is a no-op");
    check(replacement.get() == 42 && !executed, "cancelled work never executes and frees capacity");
    pool.shutdown();
    pool.shutdown();
    fails([&] { pool.submit([](std::stop_token) {}); }, "shut down");
}
void failure_and_lifetimes() {
    std::optional<tynex::task<int>> survivor;
    {
        tynex::task_pool pool(1);
        auto bad = pool.submit([](std::stop_token) -> int { throw std::runtime_error("worker failure"); });
        fails([&] { bad.get(); }, "worker failure");
        check(bad.status() == tynex::task_status::failed, "failure status");
        survivor = pool.submit([owned = std::make_unique<int>(42)](std::stop_token) { return *owned; });
        check(survivor->get() == 42, "move-only callable");
        auto nested = pool.submit([&](std::stop_token) {
            auto child = pool.submit([](std::stop_token) { return 1; });
            fails([&] { child.get(); }, "own task pool");
            fails([&] { pool.shutdown(); }, "outside a worker");
        });
        nested.get();
    }
    check(survivor->get() == 42, "completed result survives pool destruction");
    std::optional<tynex::task<void>> pending;
    {
        tynex::task_pool pool(1);
        std::promise<void> entered;
        auto work = pool.submit([&](std::stop_token stop) {
            entered.set_value();
            while (!stop.stop_requested()) std::this_thread::yield();
        });
        check(entered.get_future().wait_for(5s) == std::future_status::ready, "work starts before shutdown");
        pending = pool.submit([](std::stop_token) { throw std::runtime_error("must not run"); });
        pool.shutdown();
        fails([&] { work.get(); }, "cancelled");
    }
    fails([&] { pending->get(); }, "cancelled");
}
void concurrent_submit_and_wait() {
    tynex::task_pool pool(4, 1024);
    std::atomic<int> total = 0;
    std::vector<std::jthread> producers;
    for (int i = 0; i < 8; ++i) producers.emplace_back([&] {
        for (int n = 0; n < 100; ++n) {
            auto work = pool.submit([](std::stop_token) { return 1; });
            total += work.get();
        }
    });
    producers.clear();
    check(total == 800, "concurrent submission and reads");
}
}
int main() {
    try {
        parallelism(); cancellation_and_capacity(); failure_and_lifetimes(); concurrent_submit_and_wait();
        fails([] { tynex::task_pool pool(0); }, "1..64");
        fails([] { tynex::task_pool pool(1, 0); }, "capacity");
        std::cout << "Task pool: OS parallelism, bounded queue, cancellation, lifetimes and concurrent callers passed\n";
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
