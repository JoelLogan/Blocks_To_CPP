// Reference solution: word and line count of a file (tests/assignments/word_count).
// The program reads only the file it names, poem.txt, relative to the folder
// it runs in. `std::getline(std::cin >> std::ws, ...)` stands for an *ask*
// block; with one answer per line, both print the same.

#include <fstream>
#include <iostream>
#include <string>
#include <vector>

// Splits `text` at every `separator`, as the *split* block's helper does:
// two separators in a row give an empty part.
std::vector<std::string> split(const std::string& text, const std::string& separator) {
    std::vector<std::string> parts = {};
    std::string::size_type start = 0;
    std::string::size_type found = text.find(separator);
    while (found != std::string::npos) {
        parts.push_back(text.substr(start, found - start));
        start = found + separator.size();
        found = text.find(separator, start);
    }
    parts.push_back(text.substr(start));
    return parts;
}

int main() {
    std::string wanted;
    std::cout << "Which word should I count? ";
    std::getline(std::cin >> std::ws, wanted);
    std::cout << '\n';
    std::ifstream file("poem.txt");
    if (!file) {
        std::cerr << "Cannot open poem.txt" << '\n';
        return 1;
    }
    int lines = 0;
    int words = 0;
    int characters = 0;
    int matches = 0;
    int longest = 0;
    int longest_length = -1;
    std::string line;
    while (std::getline(file, line)) {
        lines += 1;
        int length = static_cast<int>(line.size());
        characters += length;
        if (length > longest_length) {
            longest = lines;
            longest_length = length;
        }
        for (const auto& part : split(line, " ")) {
            if (!part.empty()) {
                words += 1;
                if (part == wanted) {
                    matches += 1;
                }
            }
        }
    }
    std::cout << "Lines: " << lines << '\n';
    std::cout << "Words: " << words << '\n';
    std::cout << "Characters (without line breaks): " << characters << '\n';
    std::cout << "Longest line: " << longest << " (" << longest_length << " characters)" << '\n';
    std::cout << "\"" << wanted << "\" appears " << matches << " times." << '\n';
    return 0;
}
