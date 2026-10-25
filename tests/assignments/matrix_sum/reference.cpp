// Reference solution: matrix sum (tests/assignments/matrix_sum).
// Each `std::cin >>` stands for an *ask* block; with one valid answer per
// line, both print the same.

#include <iostream>
#include <vector>

int main() {
    int rows = 0;
    int columns = 0;
    std::cout << "Rows: ";
    std::cin >> rows;
    std::cout << "Columns: ";
    std::cin >> columns;
    if (rows < 1 || columns < 1) {
        std::cout << "A matrix needs at least one row and one column." << '\n';
        return 1;
    }
    std::vector<std::vector<int>> matrix = {};
    for (int r = 0; r < rows; ++r) {
        std::vector<int> row = {};
        for (int c = 0; c < columns; ++c) {
            int value = 0;
            std::cout << "Row " << r + 1 << ", column " << c + 1 << ": ";
            std::cin >> value;
            row.push_back(value);
        }
        matrix.push_back(row);
    }
    std::cout << '\n';
    std::vector<int> column_sums = {};
    for (int c = 0; c < columns; ++c) {
        column_sums.push_back(0);
    }
    int total = 0;
    for (const auto& row : matrix) {
        int row_sum = 0;
        for (int c = 0; c < columns; ++c) {
            std::cout << row.at(c) << " ";
            row_sum += row.at(c);
            column_sums.at(c) = column_sums.at(c) + row.at(c);
        }
        std::cout << "| " << row_sum << '\n';
        total += row_sum;
    }
    std::cout << "Column sums:";
    for (const auto& sum : column_sums) {
        std::cout << " " << sum;
    }
    std::cout << '\n';
    std::cout << "Total: " << total << '\n';
    return 0;
}
