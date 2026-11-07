//go:build ignore

// Builds the release files of the provider in the layout the Terraform and OpenTofu registries read
// (https://developer.hashicorp.com/terraform/registry/providers/publishing): one zip per platform, each holding
// terraform-provider-taba_v<version>, the manifest and the SHA256SUMS of all of them. The signature of the SHA256SUMS
// (a detached GPG signature, .sig) is made by the release workflow, which has the key.
//
//	go run scripts/package.go -version 0.2.0 -out dist   (from connectors/terraform; scripts/package.sh runs it in Docker)
//
// The binaries are built with -trimpath, without cgo and without VCS stamps, and the zips carry a fixed date: the same
// commit and Go version give the same files.
package main

import (
	"archive/zip"
	"crypto/sha256"
	"encoding/hex"
	"flag"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
)

// platforms are the ones the registry offers (the list of the HashiCorp provider template, without windows/arm,
// which Go no longer builds well).
var platforms = []string{
	"darwin/amd64", "darwin/arm64",
	"freebsd/386", "freebsd/amd64", "freebsd/arm", "freebsd/arm64",
	"linux/386", "linux/amd64", "linux/arm", "linux/arm64",
	"windows/386", "windows/amd64", "windows/arm64",
}

const name = "terraform-provider-taba"

// zipDate is the date of every file in the zips (reproducible builds).
var zipDate = time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)

func main() {
	version := flag.String("version", "", "version without the v (0.2.0, 0.2.0-rc.1)")
	out := flag.String("out", "dist", "output folder (created; must not exist)")
	only := flag.String("platforms", "", "comma-separated subset of the platforms (default: all)")
	flag.Parse()
	if !regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$`).MatchString(*version) {
		log.Fatalf("-version %q is not X.Y.Z or X.Y.Z-rc.N", *version)
	}
	if _, err := os.Stat(*out); err == nil {
		log.Fatalf("%s already exists", *out)
	}
	if err := os.MkdirAll(*out, 0o755); err != nil {
		log.Fatal(err)
	}
	selected := platforms
	if *only != "" {
		selected = strings.Split(*only, ",")
	}

	work, err := os.MkdirTemp("", "tfpackage")
	if err != nil {
		log.Fatal(err)
	}
	defer os.RemoveAll(work)

	var files []string
	for _, platform := range selected {
		goos, goarch, ok := strings.Cut(platform, "/")
		if !ok {
			log.Fatalf("platform %q is not os/arch", platform)
		}
		binary := fmt.Sprintf("%s_v%s", name, *version)
		if goos == "windows" {
			binary += ".exe"
		}
		built := filepath.Join(work, goos+"_"+goarch, binary)
		cmd := exec.Command("go", "build", "-trimpath", "-buildvcs=false",
			"-ldflags", "-s -w -buildid= -X main.version="+*version, "-o", built, ".")
		cmd.Env = append(os.Environ(), "CGO_ENABLED=0", "GOOS="+goos, "GOARCH="+goarch)
		cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
		if err := cmd.Run(); err != nil {
			log.Fatalf("building %s: %v", platform, err)
		}
		archive := fmt.Sprintf("%s_%s_%s_%s.zip", name, *version, goos, goarch)
		if err := zipFile(filepath.Join(*out, archive), built); err != nil {
			log.Fatal(err)
		}
		files = append(files, archive)
		fmt.Println(archive)
	}

	manifest := fmt.Sprintf("%s_%s_manifest.json", name, *version)
	data, err := os.ReadFile("terraform-registry-manifest.json")
	if err != nil {
		log.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(*out, manifest), data, 0o644); err != nil {
		log.Fatal(err)
	}
	files = append(files, manifest)

	sort.Strings(files)
	var sums strings.Builder
	for _, f := range files {
		sum, err := sha256File(filepath.Join(*out, f))
		if err != nil {
			log.Fatal(err)
		}
		fmt.Fprintf(&sums, "%s  %s\n", sum, f)
	}
	sumsName := fmt.Sprintf("%s_%s_SHA256SUMS", name, *version)
	if err := os.WriteFile(filepath.Join(*out, sumsName), []byte(sums.String()), 0o644); err != nil {
		log.Fatal(err)
	}
	fmt.Print(sums.String())
}

// zipFile writes a zip with the one file (executable), with the fixed date.
func zipFile(archive, file string) error {
	in, err := os.Open(file)
	if err != nil {
		return err
	}
	defer in.Close()
	f, err := os.Create(archive)
	if err != nil {
		return err
	}
	w := zip.NewWriter(f)
	header := &zip.FileHeader{Name: filepath.Base(file), Method: zip.Deflate, Modified: zipDate}
	header.SetMode(0o755)
	entry, err := w.CreateHeader(header)
	if err == nil {
		_, err = io.Copy(entry, in)
	}
	if err == nil {
		err = w.Close()
	}
	if closeErr := f.Close(); err == nil {
		err = closeErr
	}
	return err
}

func sha256File(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
