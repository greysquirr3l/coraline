import Foundation

protocol Shape {
    func area() -> Double
}

class BaseFigure: Shape {
    let name: String

    init(name: String) {
        self.name = name
    }

    func area() -> Double {
        return 0.0
    }

    func label() -> String {
        return name
    }
}