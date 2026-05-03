public class BenchSum {
    public static void main(String[] args) {
        long sum = 0;
        for (long i = 1; i <= 1_000_000_000L; i++) {
            sum += i;
        }
        System.out.println("sum = " + sum);
    }
}
