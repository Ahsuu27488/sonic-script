I am setting up SonicScript, a voice dictation (speech-to-text) app: I speak Urdu, Hindi and English, mixed, and
it types what I said in Roman Urdu / Hinglish wherever my cursor is. To get my words right it needs my personal
word list: words a speech recognizer cannot guess, so it writes them correctly instead of something that only
sounds similar (for example "McCloud" when I said "Claude").

Using what you know about me (your memory and our past chats), list:
- names of people I mention (family, friends, colleagues, clients)
- my projects, products, companies, channels, apps I build
- tools, apps, brands and technical terms I use that are not everyday words
- places that matter to me
- words I spell in my own way (my Roman Urdu spellings)
Leave out ordinary words any recognizer knows. Do not invent anything: only what you actually know about me.

Write only the list, in this exact format, in one code block, nothing before or after it:
# Group name
Word
Word = how it might be misheard, another spelling

If you know little about me, say so in one "# " line and list what you do know.

Collect mode: if I say "collect", ask me for words one at a time (I may dictate them, so fix obvious spelling
mistakes in names and ask when unsure); when I say "done", print the whole list in the format above.
