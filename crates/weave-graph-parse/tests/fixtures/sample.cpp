class Base {
public:
    virtual void name();
};

class Greeter : public Base {
public:
    std::string greet() {
        return this->helper();
    }
    std::string helper() {
        return formatName();
    }
};
