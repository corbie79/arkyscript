#include "runtime/task/task_pool.h"
#include <cassert>
#include <iostream>

int main() {
    next2::luax::task_pool workers(2, 128);
    auto sum = [](int end, next2::luax::cancellation_token stop) {
        long long result = 0;
        for (int i = 1; i <= end; ++i) {
            if (i % 1024 == 0 && stop.stop_requested()) throw next2::luax::task_cancelled();
            result += i;
        }
        return result;
    };
    auto first = workers.submit([sum](next2::luax::cancellation_token stop) { return sum(10000, stop); });
    auto second = workers.submit([sum](next2::luax::cancellation_token stop) { return sum(20000, stop); });
    // Both are already submitted; the workers execute independently of this caller.
    const auto total = first.get() + second.get();
    assert(total == 250015000);
    std::cout << "Parallel task sum: " << total << '\n';
}
