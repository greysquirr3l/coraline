let
  radius = 2.0;
  area = r: 3.14159 * r * r;
in
{
  inherit radius;
  value = area radius;
}
