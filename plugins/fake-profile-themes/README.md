# Fake Profile Themes

Reads the two colors people hide in their bio with the invisible 3y3 encoding and shows them in the profile popout: a section with two color swatches and their hex values, drawn on the primary color.

The popout only reads profiles the client already has. When the profile is not loaded yet the section shows a Load button, and the click loads the profile. Profiles without hidden colors show nothing.

Settings:
- `nitroFirst`: skip the hidden colors when the profile already has an accent color from Nitro. Default on. The client does not expose theme colors, the accent color is the closest signal it has.

Limits: the popout itself is not re-themed, only this section is colored. Setting your own colors is not included, paste the 3y3 text into your bio with any other 3y3 tool.

Permissions: `ui-mount`, `context-profiles`, `storage`.

Port of the Vencord plugin fakeProfileThemes.
