#pragma once

// Fixed OS workers, bounded queue, cooperative cancellation. No Lua dependency.
#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <deque>
#include <exception>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <thread>
#include <type_traits>
#include <utility>
#include <variant>
#include <vector>

namespace next2::luax {

class task_cancelled : public std::runtime_error {
public:
    task_cancelled() : std::runtime_error("task cancelled") {}
};

namespace task_detail { struct job; }

// A read-only, shared cancellation flag. Polling is all this runtime needs;
// this also works with C++20 libraries that have no std::stop_token yet.
class cancellation_token {
    std::shared_ptr<std::atomic<bool>> requested_;
    friend struct task_detail::job;
    explicit cancellation_token(std::shared_ptr<std::atomic<bool>> flag) : requested_(std::move(flag)) {}
public:
    cancellation_token() = default;
    bool stop_requested() const noexcept { return requested_ && requested_->load(std::memory_order_acquire); }
};

enum class task_status { queued, running, cancelling, succeeded, failed, cancelled };

inline const char* task_status_name(task_status status) noexcept {
    switch (status) {
    case task_status::queued: return "queued";
    case task_status::running: return "running";
    case task_status::cancelling: return "cancelling";
    case task_status::succeeded: return "succeeded";
    case task_status::failed: return "failed";
    case task_status::cancelled: return "cancelled";
    }
    return "unknown";
}

namespace task_detail {
inline thread_local const void* current_pool = nullptr;
inline bool finished(task_status status) noexcept {
    return status == task_status::succeeded || status == task_status::failed || status == task_status::cancelled;
}

struct job {
    mutable std::mutex mutex;
    std::condition_variable changed;
    cancellation_token token{std::make_shared<std::atomic<bool>>(false)};
    task_status status = task_status::queued;
    std::exception_ptr failure;
    const void* owner = nullptr;
    virtual ~job() = default;
    virtual void execute() noexcept = 0;

    bool cancel() {
        {
            std::lock_guard lock(mutex);
            if (finished(status) || status == task_status::cancelling) return false;
            status = status == task_status::queued ? task_status::cancelled : task_status::cancelling;
            token.requested_->store(true, std::memory_order_release);
        }
        changed.notify_all();
        return true;
    }
    void wait(std::unique_lock<std::mutex>& lock) {
        if (!finished(status) && current_pool == owner)
            throw std::logic_error("a worker cannot wait for unfinished work in its own task pool");
        changed.wait(lock, [&] { return finished(status); });
        if (status == task_status::cancelled) throw task_cancelled();
        if (failure) std::rethrow_exception(failure);
    }
};

template<class R> struct result_job final : job {
    using stored_type = std::conditional_t<std::is_void_v<R>, std::monostate, R>;
    std::optional<stored_type> result;
    std::function<R(cancellation_token)> function;

    void execute() noexcept override {
        {
            std::lock_guard lock(mutex);
            if (status == task_status::cancelled) return;
            status = task_status::running;
        }
        try {
            auto value = [&]() -> stored_type {
                if constexpr (std::is_void_v<R>) { function(token); return {}; }
                else return function(token);
            }();
            std::lock_guard lock(mutex);
            if (status == task_status::cancelling) status = task_status::cancelled;
            else { result.emplace(std::move(value)); status = task_status::succeeded; }
        } catch (const task_cancelled&) {
            std::lock_guard lock(mutex);
            status = task_status::cancelled;
        } catch (...) {
            std::lock_guard lock(mutex);
            failure = std::current_exception();
            status = status == task_status::cancelling ? task_status::cancelled : task_status::failed;
        }
        // Free captured input promptly, including for handles retained by the caller.
        function = {};
        changed.notify_all();
    }
};

struct pool_state {
    std::mutex mutex;
    std::condition_variable available;
    std::deque<std::shared_ptr<job>> queue;
    std::vector<std::shared_ptr<job>> active;
    bool closed = false;
};
} // namespace task_detail

template<class R> class task {
    std::shared_ptr<task_detail::result_job<R>> job_;
    explicit task(std::shared_ptr<task_detail::result_job<R>> value) : job_(std::move(value)) {}
    friend class task_pool;
public:
    task_status status() const { std::lock_guard lock(job_->mutex); return job_->status; }
    bool ready() const { return task_detail::finished(status()); }
    bool cancel() const { return job_->cancel(); }
    // Results are copied: multiple readers may safely wait on the same handle.
    R get() const {
        std::unique_lock lock(job_->mutex);
        job_->wait(lock);
        if constexpr (!std::is_void_v<R>) return *job_->result;
    }
};

class task_pool {
    std::shared_ptr<task_detail::pool_state> state_ = std::make_shared<task_detail::pool_state>();
    std::vector<std::thread> threads_;
    std::size_t capacity_;

    static void worker(const std::shared_ptr<task_detail::pool_state>& state, std::size_t index) {
        task_detail::current_pool = state.get();
        for (;;) {
            std::shared_ptr<task_detail::job> next;
            {
                std::unique_lock lock(state->mutex);
                state->available.wait(lock, [&] { return state->closed || !state->queue.empty(); });
                if (state->closed) break;
                next = std::move(state->queue.front());
                state->queue.pop_front();
                state->active[index] = next;
            }
            next->execute();
            {
                std::lock_guard lock(state->mutex);
                state->active[index].reset();
            }
        }
        task_detail::current_pool = nullptr;
    }

    void close_and_join() noexcept {
        std::deque<std::shared_ptr<task_detail::job>> pending;
        {
            std::lock_guard lock(state_->mutex);
            state_->closed = true;
            pending.swap(state_->queue);
        }
        for (const auto& work : pending) work->cancel();
        for (std::size_t i = 0; i < state_->active.size(); ++i) {
            std::shared_ptr<task_detail::job> work;
            { std::lock_guard lock(state_->mutex); work = state_->active[i]; }
            if (work) work->cancel();
        }
        state_->available.notify_all();
        for (auto& thread : threads_) if (thread.joinable()) {
            // A captured last owner can die inside its own work. The worker's
            // shared state stays alive until the detached worker actually exits.
            if (thread.get_id() == std::this_thread::get_id()) thread.detach();
            else thread.join();
        }
    }
public:
    explicit task_pool(std::size_t workers = default_workers(), std::size_t max_pending = 4096)
        : capacity_(max_pending) {
        if (workers == 0 || workers > 64 || max_pending == 0)
            throw std::invalid_argument("task pool needs 1..64 workers and a nonzero queue capacity");
        state_->active.resize(workers);
        threads_.reserve(workers);
        try {
            for (std::size_t i = 0; i < workers; ++i)
                threads_.emplace_back([state = state_, i] { worker(state, i); });
        } catch (...) { close_and_join(); throw; }
    }
    static std::size_t default_workers() noexcept {
        return std::clamp<std::size_t>(std::thread::hardware_concurrency(), 1, 4);
    }
    task_pool(const task_pool&) = delete;
    task_pool& operator=(const task_pool&) = delete;
    ~task_pool() { close_and_join(); }

    // Lifecycle methods belong to the owner. submit and task handles are thread safe.
    void shutdown() {
        if (task_detail::current_pool == state_.get())
            throw std::logic_error("task pool shutdown must be called by its owner, outside a worker");
        close_and_join();
    }
    std::size_t worker_count() const noexcept { return threads_.size(); }

    template<class F> auto submit(F&& function) {
        using R = std::invoke_result_t<std::decay_t<F>&, cancellation_token>;
        static_assert(std::is_void_v<R> || (std::is_copy_constructible_v<R> && !std::is_reference_v<R>),
                      "Task results must be copyable values (or void)");
        auto work = std::make_shared<task_detail::result_job<R>>();
        auto callable = std::make_shared<std::decay_t<F>>(std::forward<F>(function));
        work->function = [callable](cancellation_token stop) -> R { return std::invoke(*callable, stop); };
        work->owner = state_.get();
        {
            std::lock_guard lock(state_->mutex);
            if (state_->closed) throw std::runtime_error("task pool is shut down");
            std::erase_if(state_->queue, [](const auto& item) {
                std::lock_guard item_lock(item->mutex);
                return item->status == task_status::cancelled;
            });
            if (state_->queue.size() >= capacity_) throw std::runtime_error("task queue is full");
            state_->queue.push_back(work);
        }
        state_->available.notify_one();
        return task<R>(std::move(work));
    }
};
} // namespace next2::luax
