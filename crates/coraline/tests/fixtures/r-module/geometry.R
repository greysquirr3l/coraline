area <- function(radius) {
  3.14159 * radius ^ 2
}

describe <- function(radius) {
  sprintf("area %.2f", area(radius))
}

total <- function(radii) {
  vapply(radii, area, numeric(1))
}
