const std = @import("std");

/// A circle with a radius.
pub const Circle = struct {
    radius: f64,

    /// The area of the circle.
    pub fn area(self: Circle) f64 {
        return 3.14159 * self.radius * self.radius;
    }
};

/// The total area of every shape.
pub fn total(shapes: []Circle) f64 {
    var sum: f64 = 0;
    for (shapes) |shape| {
        sum += Circle.area(shape);
    }
    return sum;
}
