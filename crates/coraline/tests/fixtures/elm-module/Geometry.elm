module Geometry exposing (area, total)

import Html exposing (text)


area : Float -> Float
area radius =
    3.14159 * radius * radius


total : List Float -> Float
total shapes =
    List.sum (List.map area shapes)
