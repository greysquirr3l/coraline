local M = {}

function M.area(radius)
  return 3.14159 * radius * radius
end

function M.describe(radius)
  return string.format("area %.2f", M.area(radius))
end

return M
