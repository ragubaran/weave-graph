abstract class Greeter {
  String greet();
}

class LoudGreeter extends Greeter {
  String name;

  LoudGreeter(this.name);

  String greet() {
    return formatName(name);
  }
}

String formatName(String name) {
  return name.trim();
}
