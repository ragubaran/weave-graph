import scala.collection.List

trait Named {
    def getName(): String
}

class Greeter(name: String) extends Named {
    def getName(): String = {
        this.name
    }

    def greet(): String = {
        formatName(this.getName())
    }
}
