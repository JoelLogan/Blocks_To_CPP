// Reference solution: student records sorted by mark (tests/assignments/student_records).
// `std::cin >>` and `std::getline(std::cin >> std::ws, ...)` stand for *ask*
// blocks; with one valid answer per line, both print the same. The sort is
// written out (a selection sort), because the *sort … by* block needs a
// lambda, which comes in M4.

#include <iostream>
#include <string>
#include <vector>

struct Student {
    std::string name;
    int mark = 0;
};

// Sorts the students from the highest mark to the lowest.
void sort_by_mark(std::vector<Student>& students) {
    int count = static_cast<int>(students.size());
    for (int i = 0; i < count; ++i) {
        int best = i;
        for (int j = i + 1; j < count; ++j) {
            if (students.at(j).mark > students.at(best).mark) {
                best = j;
            }
        }
        Student swapped = students.at(i);
        students.at(i) = students.at(best);
        students.at(best) = swapped;
    }
}

int main() {
    int count = 0;
    std::cout << "How many students? ";
    std::cin >> count;
    std::vector<Student> students = {};
    for (int i = 0; i < count; ++i) {
        std::string name;
        int mark = 0;
        std::cout << "Name: ";
        std::getline(std::cin >> std::ws, name);
        std::cout << "Mark: ";
        std::cin >> mark;
        students.push_back(Student{.name = name, .mark = mark});
    }
    sort_by_mark(students);
    std::cout << '\n';
    std::cout << "Ranking:" << '\n';
    int place = 0;
    int passed = 0;
    for (const auto& student : students) {
        place += 1;
        std::cout << place << ". " << student.name << ": " << student.mark << '\n';
        if (student.mark >= 50) {
            passed += 1;
        }
    }
    std::cout << "Passed (50 or more): " << passed << " of " << count << '\n';
    return 0;
}
