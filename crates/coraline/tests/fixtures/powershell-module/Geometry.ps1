function Get-Area {
    param($Radius)
    return 3.14159 * $Radius * $Radius
}

function Get-Total {
    param($Shapes)
    $sum = 0
    foreach ($shape in $Shapes) {
        $sum += Get-Area -Radius $shape.Radius
    }
    return $sum
}
