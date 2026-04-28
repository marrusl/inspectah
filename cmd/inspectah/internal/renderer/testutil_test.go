package renderer

import "encoding/json"

// mustMarshal marshals v to json.RawMessage, panicking on error.
// Test helper only.
func mustMarshal(v interface{}) json.RawMessage {
	data, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return json.RawMessage(data)
}
