module Geometry

using Printf
import Base: show

export area, Shape

abstract type Shape end

struct Circle <: Shape
    radius::Float64
end

function area(shape::Shape)
    if shape isa Circle
        return 3.14159 * shape.radius^2
    end
    return 0.0
end

function total(shapes::Vector{Shape})
    return sum(area.(shapes))
end

scaled(factor, shape::Shape) = factor * area(shape)

@printf("geometry ready\n")

end
