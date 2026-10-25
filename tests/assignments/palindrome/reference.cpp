// Reference solution: palindrome and string reversal (tests/assignments/palindrome).
// `std::getline(std::cin >> std::ws, ...)` stands for an *ask* block; with one
// answer per line, both print the same.

#include <cctype>
#include <iostream>
#include <string>

// The text in lowercase, as the *lowercase* block's helper gives it.
std::string to_lower(std::string text) {
    for (char& letter : text) {
        letter = static_cast<char>(std::tolower(static_cast<unsigned char>(letter)));
    }
    return text;
}

// The text backwards.
std::string reversed(const std::string& text) {
    std::string result = "";
    for (int i = static_cast<int>(text.size()) - 1; i >= 0; --i) {
        result += text.at(i);
    }
    return result;
}

// Whether the text reads the same backwards, ignoring spaces and letter case.
bool is_palindrome(const std::string& text) {
    std::string letters = "";
    for (const auto& letter : to_lower(text)) {
        if (letter != ' ') {
            letters += letter;
        }
    }
    return letters == reversed(letters);
}

int main() {
    int checked = 0;
    int palindromes = 0;
    std::cout << "Type a word or a sentence, or quit to stop." << '\n';
    while (true) {
        std::string text;
        std::cout << "Text: ";
        std::getline(std::cin >> std::ws, text);
        if (text == "quit") {
            break;
        }
        checked += 1;
        std::cout << "Reversed: " << reversed(text) << '\n';
        if (is_palindrome(text)) {
            palindromes += 1;
            std::cout << "\"" << text << "\" is a palindrome." << '\n';
        } else {
            std::cout << "\"" << text << "\" is not a palindrome." << '\n';
        }
    }
    std::cout << "Palindromes: " << palindromes << " of " << checked << '\n';
    return 0;
}
