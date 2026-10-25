// Reference solution: dice statistics (tests/assignments/dice_statistics).
// `std::cin >>` stands for an *ask* block; with one valid answer per line,
// both print the same. The rolls are random, so the expected output is a
// regular expression (stdout.regex).

#include <iomanip>
#include <iostream>
#include <map>
#include <random>

// A random whole number from `low` to `high`, including both, as the
// *random integer* block's helper gives it (seeded once, from
// `std::random_device`).
int random_int(int low, int high) {
    static std::mt19937 engine{std::random_device{}()};
    std::uniform_int_distribution<int> distribution(low, high);
    return distribution(engine);
}

int main() {
    int rolls = 0;
    std::cout << "How many times should I roll the die? ";
    std::cin >> rolls;
    std::cout << '\n';
    if (rolls < 1) {
        std::cout << "Roll at least once." << '\n';
        return 1;
    }
    std::map<int, int> counts = {};
    for (int face = 1; face <= 6; ++face) {
        counts[face] = 0;
    }
    for (int i = 0; i < rolls; ++i) {
        int face = random_int(1, 6);
        counts[face] = counts.at(face) + 1;
    }
    std::cout << std::fixed << std::setprecision(1);
    int total = 0;
    int most_common = 1;
    for (const auto& [face, count] : counts) {
        std::cout << face << ": " << count << " (" << count * 100.0 / rolls << "%)" << '\n';
        total += count;
        if (count > counts.at(most_common)) {
            most_common = face;
        }
    }
    std::cout << "Faces: " << counts.size() << '\n';
    std::cout << "Total: " << total << '\n';
    std::cout << "Most common face: " << most_common << '\n';
    return 0;
}
