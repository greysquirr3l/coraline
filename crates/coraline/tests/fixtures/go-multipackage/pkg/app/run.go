package app

import "example.com/project/pkg/util"

type Runner struct {
	Prefix string
}

func Run() string {
	area := util.Area(2.0)
	return util.Format(area)
}