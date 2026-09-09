from collections import OrderedDict


class Greeter:
    def __init__(self, name):
        self.name = name

    def greet(self):
        return format_name(self.name)


class LoudGreeter(Greeter):
    def greet(self):
        return self.greet().upper()


def format_name(name):
    return name.strip()
