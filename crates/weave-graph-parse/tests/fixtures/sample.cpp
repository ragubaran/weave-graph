#include <string>

int add(int a, int b) {
    return a + b;
}

namespace app {

struct Point {
    int x;
    int y;
};

class Base {
public:
    virtual void name();
};

class Greeter : public Base {
public:
    std::string greet() {
        Base::name();
        return this->helper();
    }
    std::string helper() {
        return formatName();
    }
    int* getPtr() {
        return nullptr;
    }
};

}

struct {
    int anon_field;
};

namespace {
    void secret() {}
}
