#include <cstdio>

int divide(int total, int parts) {
    int total_copy = total;
    {
        int total = parts;
        (void)total;
    }
    double ratio = total_copy;
    return ratio / parts;
}

int main() {
    std::printf("%d\n", divide(7, 2));
    return 0;
}
