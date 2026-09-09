const std = @import("std");

const Greeter = struct {
    name: []const u8,

    pub fn greet(self: Greeter) []const u8 {
        return formatName(self.name);
    }
};

pub fn formatName(name: []const u8) []const u8 {
    return name;
}
