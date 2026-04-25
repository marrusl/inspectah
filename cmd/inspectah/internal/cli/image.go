package cli

import (
	"context"
	"fmt"
	"os"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/version"
	"github.com/spf13/cobra"
)

func newImageCmd(opts *GlobalOpts) *cobra.Command {
	imageCmd := &cobra.Command{
		Use:   "image",
		Short: "Manage the inspectah container image",
	}

	imageCmd.AddCommand(newImageInfoCmd(opts))
	imageCmd.AddCommand(newImageUpdateCmd(opts))
	imageCmd.AddCommand(newImageUseCmd())

	return imageCmd
}

func newImageInfoCmd(opts *GlobalOpts) *cobra.Command {
	return &cobra.Command{
		Use:   "info",
		Short: "Show current image reference and pin status",
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Printf("Resolved image: %s\n", opts.Image)

			pinned := container.LoadPinnedImage()
			if pinned != "" {
				fmt.Printf("Pinned:         %s\n", pinned)
			} else {
				fmt.Println("Pinned:         (none)")
			}

			fmt.Printf("Default:        %s\n", version.DefaultImageRef())
			return nil
		},
	}
}

func newImageUpdateCmd(opts *GlobalOpts) *cobra.Command {
	var latest bool

	cmd := &cobra.Command{
		Use:   "update",
		Short: "Pull the latest inspectah container image",
		RunE: func(cmd *cobra.Command, args []string) error {
			image := opts.Image
			if latest {
				image = version.DefaultImageRef()
			}

			runner := container.NewRealRunner()
			return container.EnsureImage(context.Background(), runner, image, "always", os.Stderr)
		},
	}

	cmd.Flags().BoolVar(&latest, "latest", false, "pull latest regardless of pin")

	return cmd
}

func newImageUseCmd() *cobra.Command {
	var unpin bool

	cmd := &cobra.Command{
		Use:   "use <image-ref>",
		Short: "Pin a specific image version for future runs",
		Long: `Pin a container image reference so all future inspectah commands
use this version. Accepts both v-prefixed (v0.5.1) and bare (0.5.1)
versions — the v prefix is stripped for registry tags.

To unpin, run: inspectah image use --unpin`,
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if unpin {
				if err := container.SavePinnedImage(""); err != nil {
					return fmt.Errorf("failed to clear pin: %w", err)
				}
				fmt.Println("Image pin cleared. Using default image.")
				return nil
			}

			if len(args) == 0 {
				return fmt.Errorf("specify an image reference, or use --unpin to clear")
			}

			ref := args[0]
			if err := container.SavePinnedImage(ref); err != nil {
				return fmt.Errorf("failed to save pin: %w", err)
			}
			fmt.Printf("Pinned: %s\n", ref)
			fmt.Println("All future runs will use this image until changed or unpinned.")
			return nil
		},
	}

	cmd.Flags().BoolVar(&unpin, "unpin", false, "clear the pinned image")
	return cmd
}
