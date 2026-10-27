defmodule Geometry do
  alias Other.Module, as: Other

  defmodule Circle do
    defstruct [:radius]

    def area(%Circle{radius: radius}) do
      3.14159 * radius * radius
    end
  end

  def total(shapes) do
    describe(shapes)
  end

  defp describe(shapes) do
    Enum.map(shapes, &Circle.area/1)
  end
end
