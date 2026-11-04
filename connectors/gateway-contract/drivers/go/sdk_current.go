//go:build !oldsdk

package main

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/sns"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
	sqstypes "github.com/aws/aws-sdk-go-v2/service/sqs/types"
)

// The SDKs from the end of 2023 on take the system attributes of ReceiveMessage in MessageSystemAttributeNames.
func setSystemAttributes(in *sqs.ReceiveMessageInput, names []string) {
	for _, n := range names {
		in.MessageSystemAttributeNames = append(in.MessageSystemAttributeNames, sqstypes.MessageSystemAttributeName(n))
	}
}

// The endpoint in the code, for the SDKs that do not read AWS_ENDPOINT_URL_SQS (the environment-variable case is the
// default of these SDKs and needs no code at all).
func setEndpointSQS(o *sqs.Options, url string) { o.BaseEndpoint = aws.String(url) }
func setEndpointSNS(o *sns.Options, url string) { o.BaseEndpoint = aws.String(url) }
