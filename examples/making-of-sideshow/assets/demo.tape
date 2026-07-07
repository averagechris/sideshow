Output "/tmp/sideshow-vhs/demo.gif"
Output "/tmp/sideshow-vhs/demo.webm"
Output "/tmp/sideshow-vhs/demo.mp4"

Set Shell "zsh"
Set Width 1600
Set Height 900
Set FontSize 30
Set Framerate 24
Set Theme "Dracula"
Set Padding 30
Set TypingSpeed 45ms

Hide
Type "export PATH=/tmp/sideshow-vhs/sideshow-build/bin:/tmp/sideshow-vhs/tw/bin:$PATH"
Enter
Type "rm -rf /tmp/sideshow-vhs/scratch && mkdir -p /tmp/sideshow-vhs/scratch && cd /tmp/sideshow-vhs/scratch"
Enter
Type "clear"
Enter
Show

Sleep 500ms
Type "sideshow new demo --theme terminal"
Enter
Sleep 1200ms

Type "ls demo/slides/"
Enter
Sleep 1300ms

Type "sideshow check demo"
Enter
Sleep 1600ms

Type "sideshow build demo"
Enter
Sleep 2200ms

Type "ls -lh demo/dist/"
Enter
Sleep 3200ms
