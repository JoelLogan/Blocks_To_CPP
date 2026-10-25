// Reference solution: grade average and maximum (tests/assignments/grade_average).
// Each `std::cin >>` stands for an *ask* block; with one valid answer per
// line, both print the same.

#include <iomanip>
#include <iostream>
#include <vector>

int main() {
    std::vector<int> grades = {};
    std::cout << "Enter grades from 0 to 100, and -1 to finish." << '\n';
    while (true) {
        int grade = 0;
        std::cout << "Grade: ";
        std::cin >> grade;
        if (grade == -1) {
            break;
        }
        if (grade < 0 || grade > 100) {
            std::cout << "A grade must be from 0 to 100." << '\n';
            continue;
        }
        grades.push_back(grade);
    }
    std::cout << '\n';
    if (grades.empty()) {
        std::cout << "No grades." << '\n';
        return 0;
    }
    std::cout << "Grades:";
    int total = 0;
    int highest = grades.at(0);
    for (const auto& grade : grades) {
        std::cout << " " << grade;
        total += grade;
        if (grade > highest) {
            highest = grade;
        }
    }
    std::cout << '\n';
    double average = static_cast<double>(total) / static_cast<double>(grades.size());
    int above = 0;
    for (const auto& grade : grades) {
        if (grade > average) {
            above += 1;
        }
    }
    std::cout << std::fixed << std::setprecision(2);
    std::cout << "Count: " << grades.size() << '\n';
    std::cout << "Average: " << average << '\n';
    std::cout << "Highest: " << highest << '\n';
    std::cout << "Above the average: " << above << '\n';
    return 0;
}
