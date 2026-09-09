import java.util.List;

interface Named {
    String getName();
}

class Greeter implements Named {
    private String name;

    public String getName() {
        return this.name;
    }

    public String greet() {
        return formatName(this.getName());
    }
}

class LoudGreeter extends Greeter {
}
