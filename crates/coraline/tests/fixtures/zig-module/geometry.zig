const std = @import("std");

pub const Circle = struct {
    radius: f64,

    pub fn area(self: Circle) f64 {
        return 3.14159 * self.radius * self.radius;
    }
};

pub fn total(shapes: []Circle) f64 {
    var sum: f64 = 0;
    for (shapes) |shape| {
        sum += Circle.area(shape);
    }
    return sum;
}
