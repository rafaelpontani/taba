package taba

import (
	"encoding/json"
	"os"
	"strings"
	"testing"

	shim "github.com/pulumi/pulumi-terraform-bridge/v3/pkg/tfshim"
)

// pulumiName turns a Terraform attribute name (snake_case) into the Pulumi one (camelCase).
func pulumiName(name string) string {
	parts := strings.Split(name, "_")
	for i := 1; i < len(parts); i++ {
		parts[i] = strings.ToUpper(parts[i][:1]) + parts[i][1:]
	}
	return strings.Join(parts, "")
}

type pulumiSchema struct {
	Config    struct{ Variables map[string]schemaProp }       `json:"config"`
	Provider  struct{ InputProperties map[string]schemaProp } `json:"provider"`
	Resources map[string]struct {
		InputProperties map[string]schemaProp `json:"inputProperties"`
		Properties      map[string]schemaProp `json:"properties"`
	} `json:"resources"`
	Functions map[string]json.RawMessage `json:"functions"`
}

type schemaProp struct {
	Secret bool `json:"secret"`
}

func loadSchema(t *testing.T) pulumiSchema {
	t.Helper()
	raw, err := os.ReadFile("cmd/pulumi-resource-taba/schema.json")
	if err != nil {
		t.Fatal(err)
	}
	var s pulumiSchema
	if err := json.Unmarshal(raw, &s); err != nil {
		t.Fatal(err)
	}
	return s
}

// UC: UC-IAC-024
// Every resource and data source of the Terraform provider has a Pulumi token: one that only the Terraform provider
// gained would show up here as missing from the schema.
func TestEveryTerraformResourceAndDataSourceIsMapped(t *testing.T) {
	prov := Provider()
	schema := loadSchema(t)
	prov.P.ResourcesMap().Range(func(key string, _ shim.Resource) bool {
		info := prov.Resources[key]
		if info == nil || info.Tok == "" {
			t.Errorf("resource %s has no Pulumi token", key)
			return true
		}
		if _, ok := schema.Resources[string(info.Tok)]; !ok {
			t.Errorf("resource %s (%s) is not in schema.json: run scripts/generate.sh", key, info.Tok)
		}
		return true
	})
	prov.P.DataSourcesMap().Range(func(key string, _ shim.Resource) bool {
		info := prov.DataSources[key]
		if info == nil || info.Tok == "" {
			t.Errorf("data source %s has no Pulumi token", key)
			return true
		}
		if _, ok := schema.Functions[string(info.Tok)]; !ok {
			t.Errorf("data source %s (%s) is not in schema.json: run scripts/generate.sh", key, info.Tok)
		}
		return true
	})
}

// UC: UC-IAC-027
// The name of a queue, distributor or token is a contract with the applications and is how createOrUpdate finds an
// existing one: nothing may generate it (no autonaming), and it stays required.
func TestNoResourceHasAutonaming(t *testing.T) {
	prov := Provider()
	for key, info := range prov.Resources {
		for field, f := range info.Fields {
			if f != nil && f.Default != nil && f.Default.AutoNamed {
				t.Errorf("%s.%s has autonaming", key, field)
			}
		}
	}
	for _, key := range []string{"taba_queue", "taba_distributor", "taba_token"} {
		tok := prov.Resources[key].Tok
		if _, ok := loadSchema(t).Resources[string(tok)]; !ok {
			t.Fatalf("%s missing from the schema", tok)
		}
	}
	raw, _ := os.ReadFile("cmd/pulumi-resource-taba/schema.json")
	var full struct {
		Resources map[string]struct {
			Required []string `json:"requiredInputs"`
		} `json:"resources"`
	}
	if err := json.Unmarshal(raw, &full); err != nil {
		t.Fatal(err)
	}
	for tok, r := range full.Resources {
		found := false
		for _, n := range r.Required {
			found = found || n == "name"
		}
		if !found {
			t.Errorf("%s: name is not a required input", tok)
		}
	}
}

// UC: UC-IAC-028
// Everything the Terraform provider marks as sensitive is a Pulumi secret (the token the server shows once, the
// provider's own token).
func TestSensitiveFieldsAreSecrets(t *testing.T) {
	prov := Provider()
	schema := loadSchema(t)
	sensitive := 0
	prov.P.ResourcesMap().Range(func(key string, r shim.Resource) bool {
		tok := string(prov.Resources[key].Tok)
		r.Schema().Range(func(attr string, s shim.Schema) bool {
			if !s.Sensitive() {
				return true
			}
			sensitive++
			name := pulumiName(attr)
			if f := prov.Resources[key].Fields[attr]; f != nil && f.Name != "" {
				name = f.Name
			}
			if !schema.Resources[tok].Properties[name].Secret {
				t.Errorf("%s.%s is sensitive in Terraform but not a secret in Pulumi", tok, attr)
			}
			return true
		})
		return true
	})
	prov.P.Schema().Range(func(attr string, s shim.Schema) bool {
		if s.Sensitive() {
			sensitive++
			if !schema.Config.Variables[pulumiName(attr)].Secret {
				t.Errorf("config %s is sensitive in Terraform but not a secret in Pulumi", attr)
			}
		}
		return true
	})
	if sensitive < 2 {
		t.Errorf("found %d sensitive fields, expected at least the token and the provider token", sensitive)
	}
}

// UC: UC-IAC-029
// A queue or distributor is replaced by deleting first: the name is the id, so creating the new one first would fail
// with topic.already-exists. A token (the id is the server's) is created first.
func TestReplacementOrder(t *testing.T) {
	prov := Provider()
	for key, want := range map[string]bool{"taba_queue": true, "taba_distributor": true, "taba_token": false} {
		if got := prov.Resources[key].DeleteBeforeReplace; got != want {
			t.Errorf("%s DeleteBeforeReplace = %v, want %v", key, got, want)
		}
	}
}
