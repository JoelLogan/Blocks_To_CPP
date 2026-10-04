#include <iostream>
#include <string>
#include "broken.hpp"

struct Point {
    int x;
    int y;
};

int main() {
    int unused = 3;
    Point p{1, 2};
    std::cout << p << "\n";
    undeclared_name = 4;
    std::string s = 5;
    unsigned int count = 2;
    if (count < -1) {
        return 1;
    }
    return 0;
}
