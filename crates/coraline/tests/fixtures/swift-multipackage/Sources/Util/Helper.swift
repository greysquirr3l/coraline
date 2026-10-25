import Foundation
import Core

struct Point {
    let x: Double
    let y: Double
}

class Circle {
    let radius: Double

    init(radius: Double) {
        self.radius = radius
    }

    func area() -> Double {
        return 3.14 * radius * radius
    }
}

func format(value: Double) -> String {
    return String(value)
}