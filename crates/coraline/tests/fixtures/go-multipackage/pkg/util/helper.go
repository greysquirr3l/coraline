package util

import "fmt"

// Helper formats values.
type Helper struct {
	Prefix string
}

// Format renders a value.
func Format(value float64) string {
	return fmt.Sprintf("%.2f", value)
}

// Area returns the area of a circle.
func Area(radius float64) float64 {
	return 3.14 * radius * radius
}