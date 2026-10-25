// Reference solution: input validation with exceptions (tests/assignments/input_validation).
// `std::getline(std::cin >> std::ws, ...)` stands for an *ask* block that
// saves the answer as text; with one answer per line, both print the same.

#include <iostream>
#include <stdexcept>
#include <string>

// The age written in `text`. `std::stoi` throws std::invalid_argument when
// the text does not start with a number, and std::out_of_range when the
// number does not fit in an int; a number that is not a possible age throws
// std::runtime_error.
int parse_age(const std::string& text) {
    int age = std::stoi(text);
    if (age < 0) {
        throw std::runtime_error("An age cannot be negative.");
    }
    if (age > 150) {
        throw std::runtime_error("Nobody is " + std::to_string(age) + " years old.");
    }
    return age;
}

int main() {
    int age = 0;
    int tries = 0;
    bool valid = false;
    while (!valid) {
        std::string answer;
        std::cout << "How old are you? ";
        std::getline(std::cin >> std::ws, answer);
        tries += 1;
        try {
            age = parse_age(answer);
            valid = true;
        } catch (const std::invalid_argument& e) {
            std::cout << "That is not a whole number." << '\n';
        } catch (const std::out_of_range& e) {
            std::cout << "That number is far too big." << '\n';
        } catch (const std::runtime_error& e) {
            std::cout << e.what() << '\n';
        }
    }
    std::cout << "In ten years you will be " << age + 10 << "." << '\n';
    std::cout << "That took " << tries << " tries." << '\n';
    return 0;
}
