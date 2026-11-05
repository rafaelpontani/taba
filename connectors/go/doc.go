// Package taba is the Go client of Taba (https://github.com/rafaelpontani/taba): the nine operations of the
// client API, Subscribe, consuming with a handler (with the automatic renewal of the invisibility) and the call
// retries of the common contract (doc 21). Standard library only; it builds on Go 1.7.
//
//	client, err := taba.NewClient(taba.Config{BaseURL: "http://localhost:8080", Token: token})
//	if err != nil {
//		return err
//	}
//	defer client.Close()
//	ctx := context.Background()
//	_, err = client.PublishJSON(ctx, "orders", map[string]int{"order": 9001})
//	_, err = client.ConsumeWith(ctx, "orders", &taba.ConsumeOptions{WaitTime: taba.Int(20)},
//		func(mc *taba.MessageContext) error {
//			fmt.Println(mc.Message.BodyText())
//			return nil // the message is deleted; returning an error keeps it
//		})
//
// The Client is safe for concurrent use. Every call takes a context.Context: cancelling it ends the call in
// flight (a long polling does not wait for its timeout).
package taba
