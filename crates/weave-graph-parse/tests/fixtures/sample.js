import { pad } from "./strings";

class Greeter {
    constructor(name) {
        this.name = name;
    }

    greet() {
        return formatName(this.name);
    }
}

class LoudGreeter extends Greeter {
    greet() {
        return this.greet().toUpperCase();
    }
}

function formatName(name) {
    return pad(name);
}
