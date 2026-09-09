package main

import "fmt"

type Greeter struct {
	Name string
}

func (g *Greeter) Greet() string {
	return formatName(g.Name)
}

func formatName(name string) string {
	fmt.Println(name)
	return name
}

type Named interface {
	GetName() string
}
