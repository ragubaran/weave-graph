using System;

namespace App {
    interface INamed {
        string GetName();
    }

    class Greeter : INamed {
        public string GetName() {
            return this.name;
        }

        public string Greet() {
            return FormatName(this.GetName());
        }
    }
}
