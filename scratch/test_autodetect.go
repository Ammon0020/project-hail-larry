package main

import (
	"encoding/json"
	"fmt"

	"github.com/adama/local-agent/internal/acp"
)

func main() {
	detected := acp.Autodetect()
	b, _ := json.MarshalIndent(detected, "", "  ")
	fmt.Printf("Detected Agents:\n%s\n", string(b))
}
