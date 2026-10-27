module Geometry (area, total) where

import Data.List (sortBy)

class Describable a where
  describe :: a -> String

data Shape
  = Circle Double
  | Square Double
  deriving (Describable)

area :: Shape -> Double
area shape = describe shape

total :: [Shape] -> Double
total shapes = area (head shapes)

sorted :: [Shape] -> [Shape]
sorted shapes = Data.List.sortBy compare shapes
