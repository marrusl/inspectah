package container

import (
	"encoding/json"
	"os"
	"path/filepath"
)

func ResolveImage(flagValue, envValue, pinnedValue, defaultValue string) string {
	if flagValue != "" {
		return flagValue
	}
	if envValue != "" {
		return envValue
	}
	if pinnedValue != "" {
		return pinnedValue
	}
	return defaultValue
}

const configFileName = "config.json"

type Config struct {
	PinnedImage string `json:"pinned_image,omitempty"`
}

func ConfigDir() string {
	if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
		return filepath.Join(xdg, "inspectah")
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "inspectah")
}

func ConfigPath() string {
	return filepath.Join(ConfigDir(), configFileName)
}

func LoadPinnedImage() string {
	data, err := os.ReadFile(ConfigPath())
	if err != nil {
		return ""
	}
	var cfg Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		return ""
	}
	return cfg.PinnedImage
}

func SavePinnedImage(image string) error {
	dir := ConfigDir()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	cfg := Config{PinnedImage: image}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(ConfigPath(), data, 0o644)
}
