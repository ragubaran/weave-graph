class Greeter
    def initialize(name)
        @name = name
    end

    def greet
        format_name(@name)
    end
end

class LoudGreeter < Greeter
end

def format_name(name)
    name.strip
end
