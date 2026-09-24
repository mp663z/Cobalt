# Browser

Read web pages the way you read a book: laid out in pages, turned with a tap.

<table>
<tr>
<td width="50%" valign="top"><img width="300" src="screenshots/index.png" alt="The sample pages"><br>The sample pages</td>
<td width="50%" valign="top"><img width="300" src="screenshots/article.png" alt="An article laid out in pages"><br>An article laid out in pages</td>
</tr>
<tr>
<td width="50%" valign="top"><img width="300" src="screenshots/links.png" alt="Every link on a page, in a list"><br>Every link on a page, in a list</td>
<td width="50%" valign="top"><img width="300" src="screenshots/fragment.png" alt="A link to a section opens on its page"><br>A link to a section opens on its page</td>
</tr>
</table>

## Features

- Pages are cut to fit the screen and the type size, and turned with a tap on
  either side. The position is shown at the bottom.
- Headings, paragraphs, lists, quotations, code and tables are kept. Scripts,
  styles, frames, video and advertising are dropped.
- Pages load over the network. A page that fails to arrive says why, and
  offers Try again when that could help.
- Tap a link to follow it. **Links** lists every link in the page, the ones on
  the page you are reading first.
- **Back** and **Forward** return to where you were, on the same page.
- **Go to** takes an address, or words to search for.
- A link to a section opens on the page that section starts on.

## Limits

There is no JavaScript. Images are not drawn yet; a linked image is offered
as a link. Pages are read up to 2 MB. The runtime reports only the body of a
response, so relative links on a page that was redirected are resolved from
the address that was asked for, unless the page names its own base.
