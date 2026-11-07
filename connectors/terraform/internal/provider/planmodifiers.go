package provider

import (
	"context"
	"fmt"

	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
)

// effectiveModifier plans an effective_* attribute from the field it follows: the declared value when there is one;
// the value in the state when the field was and still is the node default; unknown otherwise (the server says).
// Without it the attribute would be "known after apply" on every change, or wrong after a change of the field.
type effectiveModifier struct {
	field string
}

func (m effectiveModifier) Description(context.Context) string {
	return fmt.Sprintf("Follows %s: the declared value, or the node default.", m.field)
}

func (m effectiveModifier) MarkdownDescription(ctx context.Context) string { return m.Description(ctx) }

func (m effectiveModifier) PlanModifyInt64(ctx context.Context, req planmodifier.Int64Request, resp *planmodifier.Int64Response) {
	var declared types.Int64
	resp.Diagnostics.Append(req.Config.GetAttribute(ctx, path.Root(m.field), &declared)...)
	if resp.Diagnostics.HasError() {
		return
	}
	switch {
	case declared.IsUnknown():
		resp.PlanValue = types.Int64Unknown()
	case !declared.IsNull():
		resp.PlanValue = declared
	case req.State.Raw.IsNull():
		resp.PlanValue = types.Int64Unknown()
	default:
		var prior types.Int64
		resp.Diagnostics.Append(req.State.GetAttribute(ctx, path.Root(m.field), &prior)...)
		if prior.IsNull() && !req.StateValue.IsNull() {
			resp.PlanValue = req.StateValue
		} else {
			resp.PlanValue = types.Int64Unknown()
		}
	}
}

// idleTimeoutModifier plans idle_timeout when it is not declared: the server default on a volatile queue, null on
// any other. So removing it from the configuration goes back to the default, which a plain Optional+Computed
// attribute would not do (it would keep the old value).
type idleTimeoutModifier struct{}

func (idleTimeoutModifier) Description(context.Context) string {
	return fmt.Sprintf("Not declared: %d on a volatile queue, null on any other.", defaultIdleTimeout)
}

func (m idleTimeoutModifier) MarkdownDescription(ctx context.Context) string {
	return m.Description(ctx)
}

func (idleTimeoutModifier) PlanModifyInt64(ctx context.Context, req planmodifier.Int64Request, resp *planmodifier.Int64Response) {
	if !req.ConfigValue.IsNull() {
		return
	}
	var volatile types.Bool
	resp.Diagnostics.Append(req.Config.GetAttribute(ctx, path.Root("volatile"), &volatile)...)
	switch {
	case volatile.IsUnknown():
		resp.PlanValue = types.Int64Unknown()
	case volatile.ValueBool():
		resp.PlanValue = types.Int64Value(defaultIdleTimeout)
	default:
		resp.PlanValue = types.Int64Null()
	}
}

// deadLetterNameModifier plans dead_letter_name as <name>-dead-letter, known already at plan time.
type deadLetterNameModifier struct{}

func (deadLetterNameModifier) Description(context.Context) string {
	return "The name followed by " + deadLetterSuffix + "."
}

func (m deadLetterNameModifier) MarkdownDescription(ctx context.Context) string {
	return m.Description(ctx)
}

func (deadLetterNameModifier) PlanModifyString(ctx context.Context, req planmodifier.StringRequest, resp *planmodifier.StringResponse) {
	var name types.String
	resp.Diagnostics.Append(req.Plan.GetAttribute(ctx, path.Root("name"), &name)...)
	if name.IsUnknown() || name.IsNull() {
		resp.PlanValue = types.StringUnknown()
		return
	}
	resp.PlanValue = types.StringValue(name.ValueString() + deadLetterSuffix)
}
