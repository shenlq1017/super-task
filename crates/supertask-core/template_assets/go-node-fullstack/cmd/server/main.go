// SuperTask demo: Go backend launched by `kind: go` (package: ./cmd/server).
// SuperTask injects PORT when the service has a `port` and the environment
// does not define one, so we bind to $PORT (default 8081).
package main

import (
	"encoding/json"
	"net/http"
	"os"
	"time"
)

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
	})
	mux.HandleFunc("/", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{"service": "go-node-fullstack", "time": time.Now().Format(time.RFC3339)})
	})
	port := os.Getenv("PORT")
	if port == "" {
		port = "8081"
	}
	server := &http.Server{Addr: "127.0.0.1:" + port, Handler: mux}
	_ = server.ListenAndServe()
}
