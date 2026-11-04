//go:build oldsdk

package main

import (
	"github.com/aws/aws-sdk-go-v2/service/sns"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
	sqstypes "github.com/aws/aws-sdk-go-v2/service/sqs/types"
)

// The older SDKs only know AttributeNames (the field the application code of that time used).
func setSystemAttributes(in *sqs.ReceiveMessageInput, names []string) {
	for _, n := range names {
		in.AttributeNames = append(in.AttributeNames, sqstypes.QueueAttributeName(n))
	}
}

// The endpoint resolver of the SDKs of that time.
func setEndpointSQS(o *sqs.Options, url string) { o.EndpointResolver = sqs.EndpointResolverFromURL(url) }
func setEndpointSNS(o *sns.Options, url string) { o.EndpointResolver = sns.EndpointResolverFromURL(url) }
