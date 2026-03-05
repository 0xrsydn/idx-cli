package main

import (
	"os"

	"rubick/internal/cli"
)

func main() {
	os.Exit(cli.Run(os.Args[1:]))
}
