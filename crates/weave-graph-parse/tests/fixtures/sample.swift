protocol Named {
    func getName() -> String
}

class Greeter: Named {
    func getName() -> String {
        return self.name
    }

    func greet() -> String {
        return formatName(self.getName())
    }
}
