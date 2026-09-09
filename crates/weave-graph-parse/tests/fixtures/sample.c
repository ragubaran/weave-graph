#include <stdio.h>

struct Point {
    int x;
    int y;
};

int add(int a, int b) {
    return helper(a, b);
}

int helper(int a, int b) {
    return a + b;
}
