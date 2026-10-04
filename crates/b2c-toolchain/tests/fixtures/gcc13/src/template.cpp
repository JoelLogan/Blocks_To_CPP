#include <vector>

template <typename T>
T total(const std::vector<T>& values) {
    T sum{};
    for (const auto& value : values) {
        sum += value.size();
    }
    return sum;
}

int main() {
    std::vector<int> numbers{1, 2, 3};
    return total(numbers);
}
