package taba

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strconv"
)

// jsonFields are the fields of one JSON object as name -> text fragment, without interpreting them: a
// json.RawMessage keeps the bytes exactly as the server sent them, which is what the message body needs
// (decoding and encoding again would reformat it).
type jsonFields map[string]json.RawMessage

func parseFields(text []byte) (jsonFields, error) {
	trimmed := bytes.TrimLeft(text, " \t\r\n")
	if len(trimmed) == 0 || trimmed[0] != '{' {
		return nil, fmt.Errorf("object expected")
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(text, &fields); err != nil {
		return nil, err
	}
	return jsonFields(fields), nil
}

// raw is the field's text fragment, or nil when absent or null.
func (f jsonFields) raw(name string) []byte {
	value, ok := f[name]
	if !ok || string(value) == "null" {
		return nil
	}
	return value
}

func (f jsonFields) str(name string) (string, bool, error) {
	value := f.raw(name)
	if value == nil {
		return "", false, nil
	}
	var s string
	if err := json.Unmarshal(value, &s); err != nil {
		return "", false, fmt.Errorf("field %q is not a string", name)
	}
	return s, true, nil
}

func (f jsonFields) boolean(name string) (*bool, error) {
	value := f.raw(name)
	if value == nil {
		return nil, nil
	}
	switch string(value) {
	case "true":
		return Bool(true), nil
	case "false":
		return Bool(false), nil
	}
	return nil, fmt.Errorf("field %q is not a boolean", name)
}

func (f jsonFields) integer(name string) (*int, error) {
	value := f.raw(name)
	if value == nil {
		return nil, nil
	}
	n, err := strconv.Atoi(string(value))
	if err != nil {
		return nil, fmt.Errorf("field %q is not an integer", name)
	}
	return &n, nil
}

// arrayItems are the elements of an array fragment, each as a text fragment.
func arrayItems(fragment []byte) ([]json.RawMessage, error) {
	trimmed := bytes.TrimLeft(fragment, " \t\r\n")
	if len(trimmed) == 0 || trimmed[0] != '[' {
		return nil, fmt.Errorf("array expected")
	}
	var items []json.RawMessage
	if err := json.Unmarshal(fragment, &items); err != nil {
		return nil, err
	}
	return items, nil
}
