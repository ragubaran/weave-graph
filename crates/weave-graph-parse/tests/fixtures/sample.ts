import { pad } from "./strings";

interface Named {
    getName(): string;
}

class Greeter implements Named {
    constructor(private name: string) {}

    getName(): string {
        return this.name;
    }

    greet(): string {
        return formatName(this.getName());
    }
}

function formatName(name: string): string {
    return pad(name);
}
