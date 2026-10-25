// Reference solution: temperature table (tests/assignments/temperature_table).
// Each `std::cin >>` stands for an *ask* block; with one valid answer per
// line, both print the same.

#include <iomanip>
#include <iostream>

int main() {
    int start = 0;
    int end = 0;
    int step = 0;
    std::cout << "Celsius to Fahrenheit" << '\n';
    std::cout << "From: ";
    std::cin >> start;
    std::cout << "To: ";
    std::cin >> end;
    std::cout << "Step: ";
    std::cin >> step;
    std::cout << '\n';
    if (step < 1) {
        std::cout << "The step must be at least 1." << '\n';
        return 1;
    }
    std::cout << std::fixed << std::setprecision(1);
    int rows = 0;
    for (int celsius = start; celsius <= end; celsius += step) {
        double fahrenheit = celsius * 1.8 + 32;
        std::cout << celsius << " C = " << fahrenheit << " F" << '\n';
        rows += 1;
    }
    std::cout << rows << " rows" << '\n';
    return 0;
}
