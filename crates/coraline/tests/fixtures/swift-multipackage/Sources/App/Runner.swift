import Foundation
import Util

class Runner {
    func run() -> String {
        let circle = Circle(radius: 2.0)
        let area = circle.area()
        return format(value: area)
    }
}