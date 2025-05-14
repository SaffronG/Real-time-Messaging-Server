# A simple real-time messaging application built entirely in rust to learn more how TCP servers, Http Request, and real-time applications work

# How to use
One user will start the server using:
'''
cargo run
'''

The server will then ask for the filename for the logs file then provide the ip

Every subsequent user will use: 
'''
cargo run <username>
'''

Then using the port provided by the server, they will type in the open port address to send requests to
'''
//EXAMPLE
PROMPT: Enter server address (e.g. 127.0.0.1:PORT):
INPUT:  127.0.0.1:34808
'''

To close the application press Ctl+C or send the Kill Signal
