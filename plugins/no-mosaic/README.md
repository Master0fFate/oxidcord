# No Mosaic

Shows image and video attachments one under the other at the width of the column instead of the Discord mosaic grid.

Settings: `inlineVideo` (default on) plays video attachments inline without opening the viewer.

Limits: a message layout is either stacked or inline video, not both. When `inlineVideo` is on and the message has a video attachment, the video plays inline and the images of that message keep the default layout. Messages without a video are always stacked.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin noMosaic.
