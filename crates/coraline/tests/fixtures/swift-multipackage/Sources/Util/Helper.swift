import Foundation
import Core

/// A point in two dimensions.
struct Point {
    let x: Double
    let y: Double
}

/// A circle with a radius.
class Circle {
    let radius: Double

    init(radius: Double) {
        self.radius = radius
    }

    /// The area of the circle.
    func area() -> Double {
        return 3.14 * radius * radius
    }
}

/// Formats a value.
func format(value: Double) -> String {
    return String(value)
}
